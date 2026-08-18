use ash::vk::{self, Handle};
use openxr as xr;

use crate::math::Vec3;

use super::error::OpenXrRuntimeError;
use super::swapchain::{select_swapchain_format, OpenXrSwapchainFormat};
use super::ui_layer::OpenXrExternalVulkanContext;

pub const OPENXR_RAY_TEXTURE_WIDTH: u32 = 16;
pub const OPENXR_RAY_TEXTURE_HEIGHT: u32 = 256;
pub const OPENXR_RAY_WIDTH_METERS: f32 = 0.003;
pub const OPENXR_RAY_MIN_LENGTH_METERS: f32 = 0.05;
pub const OPENXR_RAY_MAX_LENGTH_METERS: f32 = 3.0;

const RAY_STAGING_USAGE: &str = "OpenXR ray overlay";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenXrRayHitKind {
    Miss,
    Ui,
}

#[derive(Debug, Clone, Copy)]
pub struct OpenXrRaySample {
    pub origin: Vec3,
    pub direction: Vec3,
    pub hit_kind: OpenXrRayHitKind,
    pub hit_distance: f32,
}

impl OpenXrRaySample {
    pub fn new(
        origin: Vec3,
        direction: Vec3,
        hit_kind: OpenXrRayHitKind,
        hit_distance: f32,
    ) -> Self {
        Self {
            origin,
            direction,
            hit_kind,
            hit_distance,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OpenXrRayOverlayOptions {
    pub texture_width: u32,
    pub texture_height: u32,
    pub ray_width_m: f32,
    pub min_length_m: f32,
    pub max_length_m: f32,
    pub eye_visibility: xr::EyeVisibility,
    pub layer_flags: xr::CompositionLayerFlags,
}

impl Default for OpenXrRayOverlayOptions {
    fn default() -> Self {
        Self {
            texture_width: OPENXR_RAY_TEXTURE_WIDTH,
            texture_height: OPENXR_RAY_TEXTURE_HEIGHT,
            ray_width_m: OPENXR_RAY_WIDTH_METERS,
            min_length_m: OPENXR_RAY_MIN_LENGTH_METERS,
            max_length_m: OPENXR_RAY_MAX_LENGTH_METERS,
            eye_visibility: xr::EyeVisibility::BOTH,
            layer_flags: xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA
                | xr::CompositionLayerFlags::UNPREMULTIPLIED_ALPHA,
        }
    }
}

struct RayStagingBuffer {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    mapped_ptr: *mut u8,
    size: vk::DeviceSize,
}

pub struct OpenXrRayOverlay {
    device: ash::Device,
    handle: xr::Swapchain<xr::Vulkan>,
    format: OpenXrSwapchainFormat,
    images: Vec<vk::Image>,
    staging: RayStagingBuffer,
    ready_image_index: Option<u32>,
    uploaded_hit: Option<OpenXrRayHitKind>,
    has_texture: bool,
    options: OpenXrRayOverlayOptions,
}

impl OpenXrRayOverlay {
    pub fn new(
        session: &xr::Session<xr::Vulkan>,
        context: OpenXrExternalVulkanContext<'_>,
        options: OpenXrRayOverlayOptions,
    ) -> Result<Self, OpenXrRuntimeError> {
        let texture_width = options.texture_width.max(1);
        let texture_height = options.texture_height.max(1);
        let format = select_swapchain_format(session)?;
        let handle = session
            .create_swapchain(&xr::SwapchainCreateInfo {
                create_flags: xr::SwapchainCreateFlags::EMPTY,
                usage_flags: xr::SwapchainUsageFlags::TRANSFER_DST
                    | xr::SwapchainUsageFlags::SAMPLED,
                format: format.xr,
                sample_count: 1,
                width: texture_width,
                height: texture_height,
                face_count: 1,
                array_size: 1,
                mip_count: 1,
            })
            .map_err(|result| OpenXrRuntimeError::CreateSwapchain {
                usage: "ray TRANSFER_DST | SAMPLED",
                result,
            })?;
        let images = handle
            .enumerate_images()
            .map_err(|result| OpenXrRuntimeError::EnumerateSwapchainImages { result })?
            .into_iter()
            .map(|image| vk::Image::from_raw(image as _))
            .collect::<Vec<_>>();
        let memory_properties =
            context
                .memory_properties
                .ok_or(OpenXrRuntimeError::MissingVulkanMemoryProperties {
                    usage: RAY_STAGING_USAGE,
                })?;
        let staging_size = (texture_width as vk::DeviceSize)
            .saturating_mul(texture_height as vk::DeviceSize)
            .saturating_mul(4);
        let staging = RayStagingBuffer::new(context.device, memory_properties, staging_size)?;

        Ok(Self {
            device: context.device.clone(),
            handle,
            format,
            images,
            staging,
            ready_image_index: None,
            uploaded_hit: None,
            has_texture: false,
            options: OpenXrRayOverlayOptions {
                texture_width,
                texture_height,
                ..options
            },
        })
    }

    pub fn ensure_texture(
        &mut self,
        context: OpenXrExternalVulkanContext<'_>,
        hit_kind: OpenXrRayHitKind,
    ) -> Result<(), OpenXrRuntimeError> {
        if self.uploaded_hit == Some(hit_kind) && self.has_texture {
            return Ok(());
        }

        let rgba = ray_rgba(hit_kind, self.options);
        self.upload_rgba(context, &rgba, hit_kind)
    }

    pub fn build_layer<'a>(
        &'a self,
        reference_space: &'a xr::Space,
        sample: &OpenXrRaySample,
    ) -> Option<xr::CompositionLayerQuad<'a, xr::Vulkan>> {
        if !self.has_texture {
            return None;
        }
        let _ = self.ready_image_index?;

        let length = clamp_ray_length(sample.hit_distance, self.options);
        let direction = sample.direction.normalize_or(Vec3::new(0.0, 0.0, -1.0));
        let mid = sample.origin + direction * (length * 0.5);
        let rect = xr::Rect2Di {
            offset: xr::Offset2Di { x: 0, y: 0 },
            extent: xr::Extent2Di {
                width: self.options.texture_width as i32,
                height: self.options.texture_height as i32,
            },
        };

        Some(
            xr::CompositionLayerQuad::new()
                .space(reference_space)
                .eye_visibility(self.options.eye_visibility)
                .sub_image(
                    xr::SwapchainSubImage::new()
                        .swapchain(&self.handle)
                        .image_array_index(0)
                        .image_rect(rect),
                )
                .pose(ray_pose(mid, direction))
                .size(xr::Extent2Df {
                    width: self.options.ray_width_m,
                    height: length,
                })
                .layer_flags(self.options.layer_flags),
        )
    }

    fn upload_rgba(
        &mut self,
        context: OpenXrExternalVulkanContext<'_>,
        rgba: &[u8],
        hit_kind: OpenXrRayHitKind,
    ) -> Result<(), OpenXrRuntimeError> {
        copy_rgba_to_staging(
            self.staging.mapped_ptr,
            rgba,
            upload_swizzle(self.format.vk),
        );
        let image_index = self
            .handle
            .acquire_image()
            .map_err(|result| OpenXrRuntimeError::AcquireSwapchainImage { result })?;
        self.handle
            .wait_image(xr::Duration::INFINITE)
            .map_err(|result| OpenXrRuntimeError::WaitSwapchainImage { result })?;
        let image = *self.images.get(image_index as usize).ok_or(
            OpenXrRuntimeError::SwapchainImageIndexOutOfBounds {
                index: image_index,
                len: self.images.len(),
            },
        )?;

        let upload_result = submit_one_time_commands(context, |command_buffer| unsafe {
            record_rgba_upload(
                context.device,
                command_buffer,
                image,
                self.options.texture_width,
                self.options.texture_height,
                self.staging.buffer,
                self.staging.size,
            );
        });
        let release_result = self
            .handle
            .release_image()
            .map_err(|result| OpenXrRuntimeError::ReleaseSwapchainImage { result });

        upload_result?;
        release_result?;

        self.ready_image_index = Some(image_index);
        self.uploaded_hit = Some(hit_kind);
        self.has_texture = true;
        Ok(())
    }
}

impl Drop for OpenXrRayOverlay {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().ok();
            self.device.unmap_memory(self.staging.memory);
            self.device.destroy_buffer(self.staging.buffer, None);
            self.device.free_memory(self.staging.memory, None);
        }
    }
}

impl RayStagingBuffer {
    fn new(
        device: &ash::Device,
        memory_properties: &vk::PhysicalDeviceMemoryProperties,
        size: vk::DeviceSize,
    ) -> Result<Self, OpenXrRuntimeError> {
        let buffer = unsafe {
            device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size)
                    .usage(vk::BufferUsageFlags::TRANSFER_SRC)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )
        }
        .map_err(|result| OpenXrRuntimeError::CreateStagingBuffer {
            usage: RAY_STAGING_USAGE,
            result,
        })?;

        let memory_requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
        let memory_type_index =
            find_host_visible_memory_type(memory_properties, memory_requirements).ok_or(
                OpenXrRuntimeError::NoHostVisibleMemoryType {
                    usage: RAY_STAGING_USAGE,
                },
            )?;
        let memory = unsafe {
            device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(size)
                    .memory_type_index(memory_type_index),
                None,
            )
        }
        .map_err(|result| OpenXrRuntimeError::AllocateStagingMemory {
            usage: RAY_STAGING_USAGE,
            result,
        })?;

        unsafe { device.bind_buffer_memory(buffer, memory, 0) }.map_err(|result| {
            OpenXrRuntimeError::BindStagingMemory {
                usage: RAY_STAGING_USAGE,
                result,
            }
        })?;
        let mapped_ptr = unsafe { device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty()) }
            .map_err(|result| OpenXrRuntimeError::MapStagingMemory {
                usage: RAY_STAGING_USAGE,
                result,
            })?
            .cast();

        Ok(Self {
            buffer,
            memory,
            mapped_ptr,
            size,
        })
    }
}

fn find_host_visible_memory_type(
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    memory_requirements: vk::MemoryRequirements,
) -> Option<u32> {
    (0..memory_properties.memory_type_count).find(|i| {
        let suitable = memory_requirements.memory_type_bits & (1 << i) != 0;
        let flags = memory_properties.memory_types[*i as usize].property_flags;
        suitable
            && flags.contains(vk::MemoryPropertyFlags::HOST_VISIBLE)
            && flags.contains(vk::MemoryPropertyFlags::HOST_COHERENT)
    })
}

fn submit_one_time_commands(
    context: OpenXrExternalVulkanContext<'_>,
    record: impl FnOnce(vk::CommandBuffer),
) -> Result<(), OpenXrRuntimeError> {
    let command_buffer = unsafe {
        context.device.allocate_command_buffers(
            &vk::CommandBufferAllocateInfo::default()
                .command_pool(context.command_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(1),
        )
    }
    .map_err(|result| OpenXrRuntimeError::AllocateCommandBuffer { result })?[0];

    let result = (|| {
        unsafe {
            context.device.begin_command_buffer(
                command_buffer,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
        }
        .map_err(|result| OpenXrRuntimeError::BeginCommandBuffer { result })?;
        record(command_buffer);
        unsafe { context.device.end_command_buffer(command_buffer) }
            .map_err(|result| OpenXrRuntimeError::EndCommandBuffer { result })?;
        unsafe {
            context.device.queue_submit(
                context.queue,
                &[vk::SubmitInfo::default().command_buffers(&[command_buffer])],
                vk::Fence::null(),
            )
        }
        .map_err(|result| OpenXrRuntimeError::QueueSubmit { result })?;
        unsafe { context.device.queue_wait_idle(context.queue) }
            .map_err(|result| OpenXrRuntimeError::QueueWaitIdle { result })?;
        Ok(())
    })();

    unsafe {
        context
            .device
            .free_command_buffers(context.command_pool, &[command_buffer]);
    }
    result
}

unsafe fn record_rgba_upload(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    image: vk::Image,
    width: u32,
    height: u32,
    staging: vk::Buffer,
    buffer_size: vk::DeviceSize,
) {
    let color_range = vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    };
    let barrier_to_transfer = vk::ImageMemoryBarrier::default()
        .image(image)
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .subresource_range(color_range);

    device.cmd_pipeline_barrier(
        command_buffer,
        vk::PipelineStageFlags::TOP_OF_PIPE,
        vk::PipelineStageFlags::TRANSFER,
        vk::DependencyFlags::empty(),
        &[],
        &[],
        &[barrier_to_transfer],
    );

    let region = vk::BufferImageCopy::default()
        .buffer_offset(0)
        .buffer_row_length(0)
        .buffer_image_height(0)
        .image_subresource(vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        })
        .image_offset(vk::Offset3D::default())
        .image_extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        });

    device.cmd_copy_buffer_to_image(
        command_buffer,
        staging,
        image,
        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        &[region],
    );

    let barrier_to_shader = vk::ImageMemoryBarrier::default()
        .image(image)
        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .dst_access_mask(vk::AccessFlags::SHADER_READ)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .subresource_range(color_range);

    device.cmd_pipeline_barrier(
        command_buffer,
        vk::PipelineStageFlags::TRANSFER,
        vk::PipelineStageFlags::FRAGMENT_SHADER,
        vk::DependencyFlags::empty(),
        &[],
        &[],
        &[barrier_to_shader],
    );

    let _ = buffer_size;
}

#[derive(Clone, Copy)]
enum RayUploadSwizzle {
    Rgba,
    Bgra,
}

fn upload_swizzle(format: vk::Format) -> RayUploadSwizzle {
    match format {
        vk::Format::B8G8R8A8_UNORM | vk::Format::B8G8R8A8_SRGB => RayUploadSwizzle::Bgra,
        _ => RayUploadSwizzle::Rgba,
    }
}

fn copy_rgba_to_staging(dst: *mut u8, rgba: &[u8], swizzle: RayUploadSwizzle) {
    match swizzle {
        RayUploadSwizzle::Rgba => unsafe {
            std::ptr::copy_nonoverlapping(rgba.as_ptr(), dst, rgba.len());
        },
        RayUploadSwizzle::Bgra => {
            let dst_slice = unsafe { std::slice::from_raw_parts_mut(dst, rgba.len()) };
            for (src, out) in rgba.chunks_exact(4).zip(dst_slice.chunks_exact_mut(4)) {
                out[0] = src[2];
                out[1] = src[1];
                out[2] = src[0];
                out[3] = src[3];
            }
        }
    }
}

fn clamp_ray_length(hit_distance: f32, options: OpenXrRayOverlayOptions) -> f32 {
    let min = options.min_length_m.max(0.001);
    let max = options.max_length_m.max(min);
    hit_distance.clamp(min, max)
}

fn ray_pose(mid: Vec3, direction: Vec3) -> xr::Posef {
    xr::Posef {
        orientation: orientation_for_ray(direction),
        position: xr::Vector3f {
            x: mid.x,
            y: mid.y,
            z: mid.z,
        },
    }
}

fn orientation_for_ray(direction: Vec3) -> xr::Quaternionf {
    let y_axis = direction.normalize_or(Vec3::new(0.0, 0.0, -1.0));
    let mut z_hint = Vec3::new(0.0, 1.0, 0.0);
    if y_axis.cross(z_hint).len() < 1e-4 {
        z_hint = Vec3::new(1.0, 0.0, 0.0);
    }
    let x_axis = y_axis.cross(z_hint).normalize_or(Vec3::new(1.0, 0.0, 0.0));
    let z_axis = x_axis.cross(y_axis).normalize_or(Vec3::new(0.0, 0.0, 1.0));

    rotation_matrix_to_quaternion([
        [x_axis.x, y_axis.x, z_axis.x],
        [x_axis.y, y_axis.y, z_axis.y],
        [x_axis.z, y_axis.z, z_axis.z],
    ])
}

fn rotation_matrix_to_quaternion(m: [[f32; 3]; 3]) -> xr::Quaternionf {
    let trace = m[0][0] + m[1][1] + m[2][2];
    if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        xr::Quaternionf {
            w: 0.25 * s,
            x: (m[2][1] - m[1][2]) / s,
            y: (m[0][2] - m[2][0]) / s,
            z: (m[1][0] - m[0][1]) / s,
        }
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        xr::Quaternionf {
            w: (m[2][1] - m[1][2]) / s,
            x: 0.25 * s,
            y: (m[0][1] + m[1][0]) / s,
            z: (m[0][2] + m[2][0]) / s,
        }
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        xr::Quaternionf {
            w: (m[0][2] - m[2][0]) / s,
            x: (m[0][1] + m[1][0]) / s,
            y: 0.25 * s,
            z: (m[1][2] + m[2][1]) / s,
        }
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        xr::Quaternionf {
            w: (m[1][0] - m[0][1]) / s,
            x: (m[0][2] + m[2][0]) / s,
            y: (m[1][2] + m[2][1]) / s,
            z: 0.25 * s,
        }
    }
}

fn ray_rgba(hit_kind: OpenXrRayHitKind, options: OpenXrRayOverlayOptions) -> Vec<u8> {
    let (r, g, b, base_alpha) = match hit_kind {
        OpenXrRayHitKind::Miss => (160u8, 160u8, 160u8, 102u8),
        OpenXrRayHitKind::Ui => (60u8, 220u8, 90u8, 230u8),
    };
    let w = options.texture_width.max(1) as i32;
    let h = options.texture_height.max(1) as i32;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let t = y as f32 / (h - 1).max(1) as f32;
        let alpha = (base_alpha as f32 * (1.0 - t * 0.65)) as u8;
        for x in 0..w {
            let cx = (w - 1) as f32 * 0.5;
            let dx = (x as f32 - cx).abs() / cx.max(1.0);
            let edge = (1.0 - dx).clamp(0.0, 1.0);
            let a = (alpha as f32 * edge) as u8;
            let i = ((y * w + x) * 4) as usize;
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = a;
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_visible_pixel(rgba: &[u8]) -> &[u8] {
        rgba.chunks_exact(4)
            .find(|pixel| pixel[3] > 0)
            .expect("visible pixel")
    }

    #[test]
    fn ray_rgba_has_expected_len_and_alpha() {
        let options = OpenXrRayOverlayOptions::default();
        let rgba = ray_rgba(OpenXrRayHitKind::Ui, options);
        assert_eq!(
            rgba.len(),
            (options.texture_width * options.texture_height * 4) as usize
        );
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn miss_ray_rgba_is_gray() {
        let rgba = ray_rgba(OpenXrRayHitKind::Miss, OpenXrRayOverlayOptions::default());
        let pixel = first_visible_pixel(&rgba);
        assert_eq!(&pixel[0..3], &[160, 160, 160]);
    }

    #[test]
    fn ui_ray_rgba_is_green() {
        let rgba = ray_rgba(OpenXrRayHitKind::Ui, OpenXrRayOverlayOptions::default());
        let pixel = first_visible_pixel(&rgba);
        assert_eq!(&pixel[0..3], &[60, 220, 90]);
    }

    #[test]
    fn ray_length_is_clamped() {
        let options = OpenXrRayOverlayOptions::default();
        assert_eq!(clamp_ray_length(0.001, options), options.min_length_m);
        assert_eq!(clamp_ray_length(99.0, options), options.max_length_m);
    }

    #[test]
    fn ray_pose_uses_midpoint_position() {
        let pose = ray_pose(Vec3::new(0.0, 0.0, -1.5), Vec3::new(0.0, 0.0, -1.0));
        assert!((pose.position.z + 1.5).abs() < 1e-4);
        assert!(pose.orientation.x.is_finite());
        assert!(pose.orientation.y.is_finite());
        assert!(pose.orientation.z.is_finite());
        assert!(pose.orientation.w.is_finite());
    }
}
