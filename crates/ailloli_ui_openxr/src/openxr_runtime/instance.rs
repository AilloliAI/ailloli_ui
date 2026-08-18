use openxr as xr;

use super::error::OpenXrRuntimeError;

pub(crate) struct OpenXrInstanceOptions<'a> {
    pub application_name: &'a str,
    pub engine_name: &'a str,
}

pub struct OpenXrInstance {
    pub entry: xr::Entry,
    pub instance: xr::Instance,
    pub system: xr::SystemId,
    pub blend_mode: xr::EnvironmentBlendMode,
    pub runtime_name: String,
    pub runtime_version: xr::Version,
    pub hand_tracking_supported: bool,
    pub hand_aim_supported: bool,
    pub composition_layer_cylinder_supported: bool,
}

impl OpenXrInstance {
    pub(crate) fn new(options: OpenXrInstanceOptions<'_>) -> Result<Self, OpenXrRuntimeError> {
        validate_openxr_name(
            "application_name",
            options.application_name,
            xr::sys::MAX_APPLICATION_NAME_SIZE,
        )?;
        validate_openxr_name(
            "engine_name",
            options.engine_name,
            xr::sys::MAX_ENGINE_NAME_SIZE,
        )?;

        let entry = unsafe { xr::Entry::load() }.map_err(|err| OpenXrRuntimeError::LoadEntry {
            message: err.to_string(),
        })?;

        #[cfg(target_os = "android")]
        entry
            .initialize_android_loader()
            .map_err(|result| OpenXrRuntimeError::InitializeAndroidLoader { result })?;

        let available = entry
            .enumerate_extensions()
            .map_err(|result| OpenXrRuntimeError::EnumerateExtensions { result })?;
        if !available.khr_vulkan_enable2 {
            return Err(OpenXrRuntimeError::MissingExtension(
                "XR_KHR_vulkan_enable2",
            ));
        }
        #[cfg(target_os = "android")]
        if !available.khr_android_create_instance {
            return Err(OpenXrRuntimeError::MissingExtension(
                "XR_KHR_android_create_instance",
            ));
        }

        let hand_tracking_ext_available = available.ext_hand_tracking;
        let hand_aim_ext_available = hand_tracking_ext_available && available.fb_hand_tracking_aim;
        let composition_layer_cylinder_supported = available.khr_composition_layer_cylinder;
        if composition_layer_cylinder_supported {
            log::info!("OpenXR extension XR_KHR_composition_layer_cylinder available/enabled");
        } else {
            log::warn!("OpenXR extension XR_KHR_composition_layer_cylinder unavailable");
        }

        let mut enabled = xr::ExtensionSet::default();
        enabled.khr_vulkan_enable2 = true;
        #[cfg(target_os = "android")]
        {
            enabled.khr_android_create_instance = true;
        }
        if hand_tracking_ext_available {
            enabled.ext_hand_tracking = true;
            if hand_aim_ext_available {
                enabled.fb_hand_tracking_aim = true;
            }
        }
        if composition_layer_cylinder_supported {
            enabled.khr_composition_layer_cylinder = true;
        }

        let instance = entry
            .create_instance(
                &xr::ApplicationInfo {
                    application_name: options.application_name,
                    application_version: 0,
                    engine_name: options.engine_name,
                    engine_version: 0,
                    api_version: xr::Version::new(1, 0, 0),
                },
                &enabled,
                &[],
            )
            .map_err(|result| OpenXrRuntimeError::CreateInstance { result })?;
        let properties = instance
            .properties()
            .map_err(|result| OpenXrRuntimeError::InstanceProperties { result })?;

        let system = instance
            .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
            .map_err(|result| OpenXrRuntimeError::System { result })?;

        let blend_modes = instance
            .enumerate_environment_blend_modes(system, xr::ViewConfigurationType::PRIMARY_STEREO)
            .map_err(|result| OpenXrRuntimeError::BlendModes { result })?;
        let blend_mode = blend_modes
            .first()
            .copied()
            .unwrap_or(xr::EnvironmentBlendMode::OPAQUE);

        let hand_tracking_supported = if hand_tracking_ext_available {
            instance.supports_hand_tracking(system).unwrap_or(false)
        } else {
            false
        };
        let hand_aim_supported = hand_tracking_supported && hand_aim_ext_available;

        Ok(Self {
            entry,
            instance,
            system,
            blend_mode,
            runtime_name: properties.runtime_name,
            runtime_version: properties.runtime_version,
            hand_tracking_supported,
            hand_aim_supported,
            composition_layer_cylinder_supported,
        })
    }
}

fn validate_openxr_name(
    field: &'static str,
    value: &str,
    max_bytes_with_nul: usize,
) -> Result<(), OpenXrRuntimeError> {
    if value.as_bytes().contains(&0) {
        return Err(OpenXrRuntimeError::InvalidOpenXrName {
            field,
            reason: "must not contain NUL bytes".to_string(),
        });
    }
    if value.len() >= max_bytes_with_nul {
        return Err(OpenXrRuntimeError::InvalidOpenXrName {
            field,
            reason: format!(
                "must be shorter than {max_bytes_with_nul} bytes including trailing NUL, got {}",
                value.len() + 1
            ),
        });
    }
    Ok(())
}
