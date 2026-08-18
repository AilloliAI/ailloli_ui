//! Portable application identity shared by runtime and packaging.

use crate::SvgSource;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// Version of the runtime-to-packager metadata document.
pub const APP_IDENTITY_METADATA_VERSION: u32 = 1;
/// Source path resolved by [`app_icon!`](https://docs.rs/ailloli_ui/latest/ailloli_ui/macro.app_icon.html).
pub const CONVENTIONAL_APP_ICON_PATH: &str = "src/assets/icons/icon.svg";
/// Environment variable used by `cargo-ailloli-ui` to request metadata without opening UI.
pub const AILLOLI_UI_PACKAGE_METADATA_PATH_ENV: &str = "AILLOLI_UI_PACKAGE_METADATA_PATH";
/// Legacy metadata-probe variable accepted when the Ailloli UI name is absent.
#[doc(hidden)]
pub const OCTAVUI_PACKAGE_METADATA_PATH_ENV: &str = "OCTAVUI_PACKAGE_METADATA_PATH";

/// A validated, portable reverse-DNS application identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApplicationId(String);

impl ApplicationId {
    /// Parses an ASCII lowercase reverse-DNS identifier.
    pub fn parse(value: impl Into<String>) -> Result<Self, AppIdentityError> {
        let value = value.into();
        validate_application_id(&value)?;
        Ok(Self(value))
    }

    /// Returns the canonical identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ApplicationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ApplicationId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

fn validate_application_id(value: &str) -> Result<(), AppIdentityError> {
    if value.is_empty() {
        return Err(AppIdentityError::EmptyId);
    }
    if value.len() > 255 {
        return Err(AppIdentityError::IdTooLong(value.len()));
    }
    if !value.is_ascii() {
        return Err(AppIdentityError::InvalidId(value.to_owned()));
    }
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() < 3 {
        return Err(AppIdentityError::InvalidId(value.to_owned()));
    }
    for part in parts {
        let mut chars = part.chars();
        if !matches!(chars.next(), Some('a'..='z'))
            || !chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
            || part.ends_with('-')
        {
            return Err(AppIdentityError::InvalidId(value.to_owned()));
        }
    }
    Ok(())
}

/// Embedded SVG used as an application or per-window icon.
#[derive(Debug, Clone)]
pub struct AppIcon {
    source: SvgSource,
    source_path: String,
}

impl AppIcon {
    /// Creates an icon from compile-time embedded SVG bytes.
    pub fn from_static_svg(bytes: &'static [u8], source_path: impl Into<String>) -> Self {
        Self {
            source: SvgSource::Static(bytes),
            source_path: source_path.into(),
        }
    }

    /// Creates an icon from owned SVG bytes, primarily for window overrides and tooling.
    pub fn from_svg_bytes(
        bytes: impl Into<std::sync::Arc<[u8]>>,
        source_path: impl Into<String>,
    ) -> Self {
        Self {
            source: SvgSource::Owned(bytes.into()),
            source_path: source_path.into(),
        }
    }

    /// SVG source consumable by widgets and rasterizers.
    pub fn source(&self) -> &SvgSource {
        &self.source
    }

    /// Raw SVG bytes.
    pub fn bytes(&self) -> &[u8] {
        self.source.as_bytes()
    }

    /// Logical source path retained for diagnostics and packaging.
    pub fn source_path(&self) -> &str {
        &self.source_path
    }

    /// Stable lowercase SHA-256 digest of the source bytes.
    pub fn sha256(&self) -> String {
        let digest = Sha256::digest(self.bytes());
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

/// Builder-style application identity. Validation is deliberately deferred to `run()`.
#[derive(Debug, Clone, Default)]
pub struct AppIdentity {
    id: Option<String>,
    name: Option<String>,
    icon: Option<AppIcon>,
}

impl AppIdentity {
    /// Creates an empty identity.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the reverse-DNS application id.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the public application name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the default application icon.
    pub fn icon(mut self, icon: AppIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Unvalidated id, when present.
    pub fn id_str(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// Unvalidated display name, when present.
    pub fn name_str(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Configured icon, when present.
    pub fn app_icon(&self) -> Option<&AppIcon> {
        self.icon.as_ref()
    }

    /// Validates all required fields and returns an owned identity.
    pub fn validate(&self) -> Result<ValidatedAppIdentity, AppIdentityError> {
        let id = self.id.clone().ok_or(AppIdentityError::MissingId)?;
        let id = ApplicationId::parse(id)?;
        let name = self
            .name
            .as_deref()
            .ok_or(AppIdentityError::MissingName)?
            .trim();
        if name.is_empty() {
            return Err(AppIdentityError::EmptyName);
        }
        let icon = self.icon.clone().ok_or(AppIdentityError::MissingIcon)?;
        Ok(ValidatedAppIdentity {
            id,
            name: name.to_owned(),
            icon,
        })
    }
}

/// Fully validated application identity used by runtime adapters.
#[derive(Debug, Clone)]
pub struct ValidatedAppIdentity {
    id: ApplicationId,
    name: String,
    icon: AppIcon,
}

impl ValidatedAppIdentity {
    pub fn id(&self) -> &ApplicationId {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn icon(&self) -> &AppIcon {
        &self.icon
    }

    /// Serializable metadata contract written for `cargo-ailloli-ui`.
    pub fn metadata(&self) -> AppIdentityMetadata {
        AppIdentityMetadata {
            schema_version: APP_IDENTITY_METADATA_VERSION,
            application_id: self.id.clone(),
            display_name: self.name.clone(),
            icon: AppIconMetadata {
                conventional_path: self.icon.source_path().to_owned(),
                sha256: self.icon.sha256(),
            },
        }
    }
}

/// Runtime metadata consumed by `cargo-ailloli-ui`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppIdentityMetadata {
    pub schema_version: u32,
    pub application_id: ApplicationId,
    pub display_name: String,
    pub icon: AppIconMetadata,
}

/// Icon identity embedded in [`AppIdentityMetadata`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppIconMetadata {
    pub conventional_path: String,
    pub sha256: String,
}

/// Validation errors for [`AppIdentity`] and [`ApplicationId`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AppIdentityError {
    #[error("application identity is missing `.id(...)`")]
    MissingId,
    #[error("application identity is missing `.name(...)`")]
    MissingName,
    #[error("application identity is missing `.icon(...)`")]
    MissingIcon,
    #[error("application id must not be empty")]
    EmptyId,
    #[error("application id is too long ({0} bytes, maximum 255)")]
    IdTooLong(usize),
    #[error("invalid application id `{0}`; expected lowercase reverse-DNS components")]
    InvalidId(String),
    #[error("application name must not be empty")]
    EmptyName,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10"/></svg>"#;

    #[test]
    fn validates_complete_identity_and_metadata_v1() {
        let identity = AppIdentity::new()
            .id("org.example.sample-app")
            .name("Sample App")
            .icon(AppIcon::from_static_svg(SVG, CONVENTIONAL_APP_ICON_PATH))
            .validate()
            .unwrap();
        let metadata = identity.metadata();
        assert_eq!(metadata.schema_version, 1);
        assert_eq!(metadata.application_id.as_str(), "org.example.sample-app");
        assert_eq!(metadata.display_name, "Sample App");
        assert_eq!(metadata.icon.sha256.len(), 64);
        let roundtrip: AppIdentityMetadata =
            serde_json::from_str(&serde_json::to_string(&metadata).unwrap()).unwrap();
        assert_eq!(roundtrip, metadata);
    }

    #[test]
    fn rejects_non_portable_ids() {
        for id in [
            "org.example",
            "Org.example.app",
            "org.example.my_app",
            "org..app",
            "org.example.-app",
            "org.example.app-",
            "org.exämple.app",
            "../org.example.app",
        ] {
            assert!(ApplicationId::parse(id).is_err(), "accepted {id}");
        }
    }

    #[test]
    fn reports_each_incomplete_identity() {
        assert_eq!(
            AppIdentity::new().validate().unwrap_err(),
            AppIdentityError::MissingId
        );
        assert_eq!(
            AppIdentity::new()
                .id("org.example.app")
                .validate()
                .unwrap_err(),
            AppIdentityError::MissingName
        );
        assert_eq!(
            AppIdentity::new()
                .id("org.example.app")
                .name("Example")
                .validate()
                .unwrap_err(),
            AppIdentityError::MissingIcon
        );
    }
}
