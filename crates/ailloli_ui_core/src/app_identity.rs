//! Portable application identity shared by runtime and packaging.

use crate::SvgSource;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// Version of the runtime-to-packager metadata document.
///
/// # Examples
///
/// ```
/// assert_eq!(ailloli_ui_core::APP_IDENTITY_METADATA_VERSION, 1);
/// ```
pub const APP_IDENTITY_METADATA_VERSION: u32 = 1;
/// Source path resolved by [`app_icon!`](https://docs.rs/ailloli_ui/latest/ailloli_ui/macro.app_icon.html).
///
/// # Examples
///
/// ```
/// assert_eq!(ailloli_ui_core::CONVENTIONAL_APP_ICON_PATH, "src/assets/icons/icon.svg");
/// ```
pub const CONVENTIONAL_APP_ICON_PATH: &str = "src/assets/icons/icon.svg";
/// Environment variable used by `cargo-ailloli-ui` to request metadata without opening UI.
///
/// # Examples
///
/// ```
/// assert_eq!(ailloli_ui_core::AILLOLI_UI_PACKAGE_METADATA_PATH_ENV, "AILLOLI_UI_PACKAGE_METADATA_PATH");
/// ```
pub const AILLOLI_UI_PACKAGE_METADATA_PATH_ENV: &str = "AILLOLI_UI_PACKAGE_METADATA_PATH";
/// Legacy metadata-probe variable accepted when the Ailloli UI name is absent.
///
/// # Examples
///
/// ```
/// assert_eq!(ailloli_ui_core::OCTAVUI_PACKAGE_METADATA_PATH_ENV, "OCTAVUI_PACKAGE_METADATA_PATH");
/// ```
#[doc(hidden)]
pub const OCTAVUI_PACKAGE_METADATA_PATH_ENV: &str = "OCTAVUI_PACKAGE_METADATA_PATH";

/// A validated, portable reverse-DNS application identifier.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ApplicationId;
///
/// let id = ApplicationId::parse("org.example.paint-app")?;
/// assert_eq!(id.as_str(), "org.example.paint-app");
/// # Ok::<(), ailloli_ui_core::AppIdentityError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApplicationId(String);

impl ApplicationId {
    /// Parses an ASCII lowercase reverse-DNS identifier.
    ///
    /// A valid identifier contains at least three dot-separated components.
    /// Every component starts with `a` through `z`, may then contain lowercase
    /// ASCII letters, digits, or hyphens, and does not end in a hyphen. The
    /// complete UTF-8 string is limited to 255 bytes.
    ///
    /// # Errors
    ///
    /// Returns [`AppIdentityError::EmptyId`] for an empty string,
    /// [`AppIdentityError::IdTooLong`] above 255 bytes, and
    /// [`AppIdentityError::InvalidId`] for every other grammar violation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ApplicationId;
    ///
    /// let id = ApplicationId::parse("org.example.paint-app")?;
    /// assert_eq!(id.as_str(), "org.example.paint-app");
    /// assert!(ApplicationId::parse("Example_App").is_err());
    /// # Ok::<(), ailloli_ui_core::AppIdentityError>(())
    /// ```
    pub fn parse(value: impl Into<String>) -> Result<Self, AppIdentityError> {
        let value = value.into();
        validate_application_id(&value)?;
        Ok(Self(value))
    }

    /// Returns the validated identifier exactly as supplied to [`Self::parse`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ApplicationId;
    /// let id = ApplicationId::parse("org.example.paint")?;
    /// assert_eq!(id.as_str(), "org.example.paint");
    /// # Ok::<(), ailloli_ui_core::AppIdentityError>(())
    /// ```
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

/// Checks the byte limit and component grammar used by [`ApplicationId`].
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
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::AppIcon;
///
/// let icon = AppIcon::from_static_svg(b"<svg/>", "assets/icon.svg");
/// assert_eq!(icon.bytes(), b"<svg/>");
/// assert_eq!(icon.source_path(), "assets/icon.svg");
/// ```
#[derive(Debug, Clone)]
pub struct AppIcon {
    source: SvgSource,
    source_path: String,
}

impl AppIcon {
    /// Creates an icon from compile-time embedded SVG bytes.
    ///
    /// `source_path` is diagnostic and packaging metadata; this function does
    /// not read that path or validate the SVG document.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::AppIcon;
    /// let icon = AppIcon::from_static_svg(b"<svg/>", "assets/icon.svg");
    /// assert_eq!(icon.bytes(), b"<svg/>");
    /// ```
    pub fn from_static_svg(bytes: &'static [u8], source_path: impl Into<String>) -> Self {
        Self {
            source: SvgSource::Static(bytes),
            source_path: source_path.into(),
        }
    }

    /// Creates an icon from shared owned SVG bytes.
    ///
    /// This form is useful for per-window overrides and tooling that loads the
    /// bytes dynamically. `source_path` remains descriptive: it is never read
    /// and the SVG bytes are not validated here.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::AppIcon;
    /// let icon = AppIcon::from_svg_bytes(Vec::from(&b"<svg/>"[..]), "generated.svg");
    /// assert_eq!(icon.source_path(), "generated.svg");
    /// ```
    pub fn from_svg_bytes(
        bytes: impl Into<std::sync::Arc<[u8]>>,
        source_path: impl Into<String>,
    ) -> Self {
        Self {
            source: SvgSource::Owned(bytes.into()),
            source_path: source_path.into(),
        }
    }

    /// Returns the borrowed SVG source consumed by widgets and rasterizers.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{AppIcon, SvgSource};
    /// let icon = AppIcon::from_static_svg(b"<svg/>", "icon.svg");
    /// assert!(matches!(icon.source(), SvgSource::Static(_)));
    /// ```
    pub fn source(&self) -> &SvgSource {
        &self.source
    }

    /// Returns the raw, unvalidated SVG bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::AppIcon;
    /// assert_eq!(AppIcon::from_static_svg(b"abc", "icon.svg").bytes(), b"abc");
    /// ```
    pub fn bytes(&self) -> &[u8] {
        self.source.as_bytes()
    }

    /// Returns the logical source path retained for diagnostics and packaging.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::AppIcon;
    /// assert_eq!(AppIcon::from_static_svg(b"", "assets/icon.svg").source_path(), "assets/icon.svg");
    /// ```
    pub fn source_path(&self) -> &str {
        &self.source_path
    }

    /// Computes the lowercase hexadecimal SHA-256 digest of the source bytes.
    ///
    /// The returned string always contains 64 ASCII hexadecimal characters and
    /// does not incorporate [`Self::source_path`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::AppIcon;
    /// assert_eq!(AppIcon::from_static_svg(b"", "icon.svg").sha256().len(), 64);
    /// ```
    pub fn sha256(&self) -> String {
        let digest = Sha256::digest(self.bytes());
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

/// Builder-style application identity validated before a runtime is started.
///
/// An unset field is distinct from an explicitly empty string: both are
/// rejected, but with different [`AppIdentityError`] variants. Builder calls
/// replace the previous value.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{AppIcon, AppIdentity};
///
/// let identity = AppIdentity::new()
///     .id("org.example.paint")
///     .name("Paint")
///     .icon(AppIcon::from_static_svg(b"<svg/>", "assets/icon.svg"));
/// assert_eq!(identity.id_str(), Some("org.example.paint"));
/// assert_eq!(identity.validate()?.name(), "Paint");
/// # Ok::<(), ailloli_ui_core::AppIdentityError>(())
/// ```
#[derive(Debug, Clone, Default)]
pub struct AppIdentity {
    id: Option<String>,
    name: Option<String>,
    icon: Option<AppIcon>,
}

impl AppIdentity {
    /// Creates an identity with all three required fields unset.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::AppIdentity;
    /// assert_eq!(AppIdentity::new().id_str(), None);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the candidate reverse-DNS application identifier.
    ///
    /// Grammar validation is deferred to [`Self::validate`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::AppIdentity;
    /// assert_eq!(AppIdentity::new().id("org.example.paint").id_str(), Some("org.example.paint"));
    /// ```
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the public application name, replacing any previous value.
    ///
    /// Surrounding whitespace is trimmed during [`Self::validate`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::AppIdentity;
    /// assert_eq!(AppIdentity::new().name(" Paint ").name_str(), Some(" Paint "));
    /// ```
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the default application icon, replacing any previous value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{AppIcon, AppIdentity};
    /// let identity = AppIdentity::new().icon(AppIcon::from_static_svg(b"<svg/>", "icon.svg"));
    /// assert_eq!(identity.app_icon().unwrap().bytes(), b"<svg/>");
    /// ```
    pub fn icon(mut self, icon: AppIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Returns the unvalidated identifier, or `None` when [`Self::id`] was not called.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::AppIdentity;
    /// assert_eq!(AppIdentity::new().id("org.example.paint").id_str(), Some("org.example.paint"));
    /// ```
    pub fn id_str(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// Returns the unvalidated display name, including surrounding whitespace.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::AppIdentity;
    /// assert_eq!(AppIdentity::new().name(" Paint ").name_str(), Some(" Paint "));
    /// ```
    pub fn name_str(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns the configured icon, or `None` when [`Self::icon`] was not called.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::AppIdentity;
    /// assert!(AppIdentity::new().app_icon().is_none());
    /// ```
    pub fn app_icon(&self) -> Option<&AppIcon> {
        self.icon.as_ref()
    }

    /// Validates all required fields and returns an independent owned identity.
    ///
    /// The display name is trimmed. The builder itself is only borrowed and
    /// can be corrected or reused after a failed validation.
    ///
    /// # Errors
    ///
    /// Returns the corresponding `Missing*` variant for an unset field,
    /// [`AppIdentityError::EmptyName`] for an all-whitespace name, or an
    /// identifier validation error described by [`ApplicationId::parse`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{AppIcon, AppIdentity};
    /// let identity = AppIdentity::new().id("org.example.paint").name(" Paint ")
    ///     .icon(AppIcon::from_static_svg(b"<svg/>", "icon.svg"));
    /// assert_eq!(identity.validate()?.name(), "Paint");
    /// # Ok::<(), ailloli_ui_core::AppIdentityError>(())
    /// ```
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
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{AppIcon, AppIdentity};
///
/// let validated = AppIdentity::new()
///     .id("org.example.paint")
///     .name("Paint")
///     .icon(AppIcon::from_static_svg(b"<svg/>", "assets/icon.svg"))
///     .validate()?;
/// assert_eq!(validated.id().as_str(), "org.example.paint");
/// # Ok::<(), ailloli_ui_core::AppIdentityError>(())
/// ```
#[derive(Debug, Clone)]
pub struct ValidatedAppIdentity {
    id: ApplicationId,
    name: String,
    icon: AppIcon,
}

impl ValidatedAppIdentity {
    /// Returns the validated reverse-DNS application identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{AppIcon, AppIdentity};
    /// let identity = AppIdentity::new().id("org.example.paint").name("Paint")
    ///     .icon(AppIcon::from_static_svg(b"<svg/>", "icon.svg")).validate()?;
    /// assert_eq!(identity.id().as_str(), "org.example.paint");
    /// # Ok::<(), ailloli_ui_core::AppIdentityError>(())
    /// ```
    pub fn id(&self) -> &ApplicationId {
        &self.id
    }

    /// Returns the trimmed, non-empty public display name.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{AppIcon, AppIdentity};
    /// let identity = AppIdentity::new().id("org.example.paint").name(" Paint ")
    ///     .icon(AppIcon::from_static_svg(b"<svg/>", "icon.svg")).validate()?;
    /// assert_eq!(identity.name(), "Paint");
    /// # Ok::<(), ailloli_ui_core::AppIdentityError>(())
    /// ```
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the default icon and its original diagnostic source path.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{AppIcon, AppIdentity};
    /// let identity = AppIdentity::new().id("org.example.paint").name("Paint")
    ///     .icon(AppIcon::from_static_svg(b"<svg/>", "icon.svg")).validate()?;
    /// assert_eq!(identity.icon().source_path(), "icon.svg");
    /// # Ok::<(), ailloli_ui_core::AppIdentityError>(())
    /// ```
    pub fn icon(&self) -> &AppIcon {
        &self.icon
    }

    /// Builds the versioned metadata contract written for `cargo-ailloli-ui`.
    ///
    /// The icon digest is computed on every call from its SVG bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{AppIcon, AppIdentity, APP_IDENTITY_METADATA_VERSION};
    /// let identity = AppIdentity::new().id("org.example.paint").name("Paint")
    ///     .icon(AppIcon::from_static_svg(b"<svg/>", "icon.svg")).validate()?;
    /// assert_eq!(identity.metadata().schema_version, APP_IDENTITY_METADATA_VERSION);
    /// # Ok::<(), ailloli_ui_core::AppIdentityError>(())
    /// ```
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
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{
///     AppIconMetadata, AppIdentityMetadata, ApplicationId,
///     APP_IDENTITY_METADATA_VERSION,
/// };
///
/// let metadata = AppIdentityMetadata {
///     schema_version: APP_IDENTITY_METADATA_VERSION,
///     application_id: ApplicationId::parse("org.example.paint")?,
///     display_name: "Paint".into(),
///     icon: AppIconMetadata {
///         conventional_path: "assets/icon.svg".into(),
///         sha256: "0".repeat(64),
///     },
/// };
/// assert_eq!(metadata.schema_version, 1);
/// # Ok::<(), ailloli_ui_core::AppIdentityError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppIdentityMetadata {
    /// Metadata schema version; currently [`APP_IDENTITY_METADATA_VERSION`].
    pub schema_version: u32,
    /// Validated reverse-DNS application identifier.
    pub application_id: ApplicationId,
    /// Trimmed public application name.
    pub display_name: String,
    /// Conventional icon path and content digest.
    pub icon: AppIconMetadata,
}

/// Icon identity embedded in [`AppIdentityMetadata`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::AppIconMetadata;
///
/// let icon = AppIconMetadata {
///     conventional_path: "assets/icon.svg".into(),
///     sha256: "a".repeat(64),
/// };
/// assert_eq!(icon.sha256.len(), 64);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppIconMetadata {
    /// Consumer-relative logical path recorded for packaging diagnostics.
    pub conventional_path: String,
    /// Lowercase 64-character SHA-256 digest of the embedded SVG bytes.
    pub sha256: String,
}

/// Validation errors for [`AppIdentity`] and [`ApplicationId`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{AppIdentityError, ApplicationId};
///
/// assert_eq!(
///     ApplicationId::parse("").unwrap_err(),
///     AppIdentityError::EmptyId,
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AppIdentityError {
    /// No identifier was supplied with [`AppIdentity::id`].
    #[error("application identity is missing `.id(...)`")]
    MissingId,
    /// No display name was supplied with [`AppIdentity::name`].
    #[error("application identity is missing `.name(...)`")]
    MissingName,
    /// No icon was supplied with [`AppIdentity::icon`].
    #[error("application identity is missing `.icon(...)`")]
    MissingIcon,
    /// The supplied identifier is an empty string.
    #[error("application id must not be empty")]
    EmptyId,
    /// The identifier exceeds 255 bytes; the payload is its actual byte length.
    #[error("application id is too long ({0} bytes, maximum 255)")]
    IdTooLong(usize),
    /// The payload does not satisfy the lowercase reverse-DNS grammar.
    #[error("invalid application id `{0}`; expected lowercase reverse-DNS components")]
    InvalidId(String),
    /// The supplied display name is empty after trimming whitespace.
    #[error("application name must not be empty")]
    EmptyName,
}

#[cfg(test)]
mod tests {
    //! Validates identity grammar, required builder fields, and metadata round trips.

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
