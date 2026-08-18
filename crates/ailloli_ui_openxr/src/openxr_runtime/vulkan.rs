use std::ffi::CString;

use ash::vk::{self, Handle};
use ash::{Device, Entry as VkEntry, Instance as VkInstance};
use openxr as xr;

use super::error::OpenXrRuntimeError;
use super::instance::OpenXrInstance;

pub struct OpenXrVulkanContext {
    pub vk_entry: VkEntry,
    pub vk_instance: VkInstance,
    pub vk_device: Device,
    pub physical_device: vk::PhysicalDevice,
    pub memory_properties: vk::PhysicalDeviceMemoryProperties,
    pub queue: vk::Queue,
    pub queue_family_index: u32,
    pub command_pool: vk::CommandPool,
}

impl OpenXrVulkanContext {
    pub(crate) fn new(
        xr: &OpenXrInstance,
        application_name: &str,
        engine_name: &str,
    ) -> Result<Self, OpenXrRuntimeError> {
        let vk_target_version = vk::make_api_version(0, 1, 1, 0);
        let xr_target_version = xr::Version::new(1, 1, 0);

        let requirements = xr
            .instance
            .graphics_requirements::<xr::Vulkan>(xr.system)
            .map_err(|result| OpenXrRuntimeError::GraphicsRequirements { result })?;
        if xr_target_version < requirements.min_api_version_supported
            || xr_target_version > requirements.max_api_version_supported
        {
            return Err(OpenXrRuntimeError::UnsupportedVulkanVersion {
                requested: xr_target_version,
                min: requirements.min_api_version_supported,
                max: requirements.max_api_version_supported,
            });
        }

        let vk_entry =
            unsafe { VkEntry::load() }.map_err(|err| OpenXrRuntimeError::LoadVulkanEntry {
                message: err.to_string(),
            })?;

        let app_name = CString::new(application_name).map_err(|err| {
            OpenXrRuntimeError::InvalidVulkanName {
                field: "application_name",
                message: err.to_string(),
            }
        })?;
        let engine_name =
            CString::new(engine_name).map_err(|err| OpenXrRuntimeError::InvalidVulkanName {
                field: "engine_name",
                message: err.to_string(),
            })?;
        let vk_app_info = vk::ApplicationInfo::default()
            .application_name(app_name.as_c_str())
            .engine_name(engine_name.as_c_str())
            .api_version(vk_target_version);
        let vk_instance_create_info =
            vk::InstanceCreateInfo::default().application_info(&vk_app_info);

        let vk_instance_raw = unsafe {
            xr.instance.create_vulkan_instance(
                xr.system,
                std::mem::transmute(vk_entry.static_fn().get_instance_proc_addr),
                &vk_instance_create_info as *const _ as *const _,
            )
        }
        .map_err(|result| OpenXrRuntimeError::CreateVulkanInstance { result })?
        .map_err(vk::Result::from_raw)
        .map_err(|result| OpenXrRuntimeError::CreateVulkanInstanceVk { result })?;

        let vk_instance = unsafe {
            VkInstance::load(
                vk_entry.static_fn(),
                vk::Instance::from_raw(vk_instance_raw as _),
            )
        };

        let physical_device = unsafe {
            vk::PhysicalDevice::from_raw(
                xr.instance
                    .vulkan_graphics_device(xr.system, vk_instance.handle().as_raw() as _)
                    .map_err(|result| OpenXrRuntimeError::VulkanGraphicsDevice { result })?
                    as _,
            )
        };
        let memory_properties =
            unsafe { vk_instance.get_physical_device_memory_properties(physical_device) };

        let queue_family_index = unsafe {
            vk_instance
                .get_physical_device_queue_family_properties(physical_device)
                .into_iter()
                .enumerate()
                .find_map(|(index, info)| {
                    if info.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                        Some(index as u32)
                    } else {
                        None
                    }
                })
        }
        .ok_or(OpenXrRuntimeError::NoGraphicsQueueFamily)?;

        let queue_priorities = [1.0];
        let queue_infos = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family_index)
            .queue_priorities(&queue_priorities)];
        let device_create_info = vk::DeviceCreateInfo::default().queue_create_infos(&queue_infos);

        let vk_device_raw = unsafe {
            xr.instance.create_vulkan_device(
                xr.system,
                std::mem::transmute(vk_entry.static_fn().get_instance_proc_addr),
                physical_device.as_raw() as _,
                &device_create_info as *const _ as *const _,
            )
        }
        .map_err(|result| OpenXrRuntimeError::CreateVulkanDevice { result })?
        .map_err(vk::Result::from_raw)
        .map_err(|result| OpenXrRuntimeError::CreateVulkanDeviceVk { result })?;

        let vk_device = unsafe {
            Device::load(
                vk_instance.fp_v1_0(),
                vk::Device::from_raw(vk_device_raw as _),
            )
        };
        let queue = unsafe { vk_device.get_device_queue(queue_family_index, 0) };

        let command_pool = unsafe {
            vk_device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(queue_family_index)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )
        }
        .map_err(|result| OpenXrRuntimeError::CreateCommandPool { result })?;

        Ok(Self {
            vk_entry,
            vk_instance,
            vk_device,
            physical_device,
            memory_properties,
            queue,
            queue_family_index,
            command_pool,
        })
    }

    pub(crate) fn submit_one_time_commands<F>(&self, record: F) -> Result<(), OpenXrRuntimeError>
    where
        F: FnOnce(vk::CommandBuffer),
    {
        let command_buffer = unsafe {
            self.vk_device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(self.command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )
        }
        .map_err(|result| OpenXrRuntimeError::AllocateCommandBuffer { result })?[0];

        let result = (|| {
            unsafe {
                self.vk_device.begin_command_buffer(
                    command_buffer,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )
            }
            .map_err(|result| OpenXrRuntimeError::BeginCommandBuffer { result })?;

            record(command_buffer);

            unsafe { self.vk_device.end_command_buffer(command_buffer) }
                .map_err(|result| OpenXrRuntimeError::EndCommandBuffer { result })?;

            let command_buffers = [command_buffer];
            let submit_infos = [vk::SubmitInfo::default().command_buffers(&command_buffers)];
            unsafe {
                self.vk_device
                    .queue_submit(self.queue, &submit_infos, vk::Fence::null())
            }
            .map_err(|result| OpenXrRuntimeError::QueueSubmit { result })?;

            unsafe { self.vk_device.queue_wait_idle(self.queue) }
                .map_err(|result| OpenXrRuntimeError::QueueWaitIdle { result })?;

            Ok(())
        })();

        unsafe {
            self.vk_device
                .free_command_buffers(self.command_pool, &[command_buffer]);
        }

        result
    }

    pub fn render_context(&self) -> ailloli_ui_render_vulkan::VulkanRenderContext<'_> {
        ailloli_ui_render_vulkan::VulkanRenderContext::with_memory_properties(
            &self.vk_device,
            self.queue,
            self.queue_family_index,
            self.command_pool,
            &self.memory_properties,
        )
    }
}

impl Drop for OpenXrVulkanContext {
    fn drop(&mut self) {
        unsafe {
            if self.command_pool != vk::CommandPool::null() {
                self.vk_device.destroy_command_pool(self.command_pool, None);
            }
            self.vk_device.destroy_device(None);
            self.vk_instance.destroy_instance(None);
        }
    }
}
