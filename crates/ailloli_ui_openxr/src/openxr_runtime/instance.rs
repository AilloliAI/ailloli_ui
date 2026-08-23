//! OpenXR loader, extension negotiation, system selection, and runtime identity.

use openxr as xr;

use super::error::OpenXrRuntimeError;

/// Internal names used to create the OpenXR instance.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::{OpenXrRuntime, OpenXrRuntimeOptions};
/// let options = OpenXrRuntimeOptions { application_name: "My XR app".to_string(), engine_name: "My engine".to_string(), ..Default::default() };
/// let _runtime = OpenXrRuntime::new(options)?;
/// # Ok::<(), ailloli_ui_openxr::OpenXrRuntimeError>(())
/// ```
pub(crate) struct OpenXrInstanceOptions<'a> {
    /// Application name, shorter than OpenXR's byte limit and without NULs.
    pub application_name: &'a str,
    /// Engine name, shorter than OpenXR's byte limit and without NULs.
    pub engine_name: &'a str,
}

/// Loaded OpenXR instance plus negotiated system capabilities.
///
/// The runtime and system handles remain valid for the lifetime of this value.
/// Capability booleans reflect extensions enabled during construction, not a
/// prediction about every individual input device.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::OpenXrInstance;
/// fn report(instance: &OpenXrInstance) -> (&str, bool) {
///     (&instance.runtime_name, instance.hand_tracking_supported)
/// }
/// ```
pub struct OpenXrInstance {
    /// Dynamically loaded OpenXR entry table.
    pub entry: xr::Entry,
    /// Created OpenXR instance handle.
    pub instance: xr::Instance,
    /// Selected head-mounted-display system identifier.
    pub system: xr::SystemId,
    /// First advertised stereo environment blend mode, or opaque fallback.
    pub blend_mode: xr::EnvironmentBlendMode,
    /// Runtime-reported implementation name.
    pub runtime_name: String,
    /// Runtime-reported implementation version.
    pub runtime_version: xr::Version,
    /// Whether system hand tracking and its extension are available.
    pub hand_tracking_supported: bool,
    /// Whether the FB hand-aim extension is enabled with hand tracking.
    pub hand_aim_supported: bool,
    /// Whether native cylinder composition layers are enabled.
    pub composition_layer_cylinder_supported: bool,
}

impl OpenXrInstance {
    /// Loads OpenXR and creates the instance used by the high-level runtime.
    ///
    /// The function enables Vulkan 2 integration, optional hand extensions, and
    /// optional cylinder layers. Android also initializes and enables its loader
    /// extension. The first advertised stereo blend mode is selected.
    ///
    /// # Errors
    ///
    /// Returns [`OpenXrRuntimeError`] for invalid byte-limited names, loader or
    /// extension failures, instance creation, property lookup, or HMD selection.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrRuntime, OpenXrRuntimeOptions};
    /// let runtime = OpenXrRuntime::new(OpenXrRuntimeOptions::default())?;
    /// println!("{}", runtime.xr.runtime_name);
    /// # Ok::<(), ailloli_ui_openxr::OpenXrRuntimeError>(())
    /// ```
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

/// Validates an OpenXR fixed-size, NUL-terminated UTF-8 name field.
///
/// Embedded NUL bytes are rejected. `max_bytes_with_nul` includes the trailing
/// NUL, so the string itself must be strictly shorter than that limit.
///
/// # Errors
///
/// Returns [`OpenXrRuntimeError::InvalidOpenXrName`] when `value` contains NUL or
/// its UTF-8 byte length plus the terminator exceeds `max_bytes_with_nul`.
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
